# Vue inline-template runtime emission (production parity + perf)

**Status:** deferred feature, not landed. Recorded 2026-07-19 alongside the
Vue conformance-goldens program (`crates/verter_vue_conformance`).

## The gap

Verter's SFC→JS runtime pipeline emits only the **non-inline** component
topology: a separate `function render(_ctx, _cache, $props, $setup, …)` plus
`_sfc_main.render = render` (assembled by
`verter_session::compile::assemble_vue_main_module`). The official Vue
compiler's **production** SFC topology is `compileScript({ inlineTemplate:
true })` — the render closure is returned from `setup()` directly:

```js
export default { setup(__props) { /* … */ return (_ctx, _cache) => { /* render */ } } }
```

The two shapes are **behaviorally equivalent** (same component semantics);
they are **structurally different**, and official ships the inline shape in
production builds (fewer closures, no intermediate bindings object, better
minification).

Evidence in-tree:

- `CodegenOptions.inline` (`crates/verter_compiler/src/compile/types.rs`) is
  documented but **never consulted** on the `compile()` path —
  `crates/verter_compiler/src/compile/mod.rs:495` and `:790` force non-inline
  (`inline_template: false`, `is_inline: false`), and the Vue carrier bridge
  pins `inline: Some(false)`
  (`crates/verter_compiler/src/framework_common/vue_bridge.rs:450`) because the
  host assembles a standalone `function render()`.
- `crates/verter_compiler/src/script/process.rs:335-363` carries
  inline-template helpers the public pipeline never wires up.

## Why it matters

- **Production parity:** official production bundles use the inline topology;
  Verter cannot currently emit it, so byte/structure-level comparisons
  against official production output diverge on shape alone.
- **Perf:** the inline shape avoids the `__returned__` bindings object and
  one closure indirection per component.

## Conformance context

The conformance oracle
(`packages/vue-conformance-oracle/gen-vue-goldens.mjs`) vendors the official
**non-inline** goldens (the shape Verter ships today), so the seed
conformance run compares apples-to-apples. When inline-template lands, add an
`inlineTemplate: true` golden variant and a mode-tagged conformance
disposition rather than weakening the comparator.

## Acceptance sketch

- `CodegenOptions.inline` (or an explicit `inline_template` knob) drives the
  script pipeline to emit the setup-returned render closure; the host
  assembler gains the inline shape (no separate `render` attach).
- The render codegen must emit closure-scope binding access (no `$setup.`
  prefix) in inline mode — the carrier bridge comment documents this
  constraint today.
- Conformance: inline-topology cells compare against `inlineTemplate: true`
  goldens with zero divergence beyond the tracked backlog.
