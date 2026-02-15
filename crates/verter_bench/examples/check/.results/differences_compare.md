## 1_index.render.dev

Date: 2026-02-07

### Summary

- Category guess: C
- Why deferred: Requires known batch feature work for caching pattern parity (`_cache[n] || (_cache[n] = ...)`) and hoist shape alignment.
- Suspected modules: `crates/verter_core/src/codegen/vue/template/element.rs`, `crates/verter_core/src/codegen/vue/template_plugin.rs`

### Vue output snippet

```js
_createElementVNode("div", _hoisted_2, _toDisplayString(_ctx.props.describeText), 1 /* TEXT */);
```

### Verter output snippet

```js
_cache[0] ||
  (_cache[0] = _createElementVNode(
    "div",
    { class: "mt-[29px] text-[15px] text-white" },
    _toDisplayString(_ctx.props.describeText),
    1 /* TEXT */,
    -1 /* CACHED */,
  ));
```

### Observed diffs

- Verter caches a vnode that Vue does not cache in this dev render output.
- Verter emits cached patch marker (`-1 /* CACHED */`) not present in Vue output.
- Static props hoist alignment diverges as a consequence of the caching choice.

## 2_index.render.dev

Date: 2026-02-07

### Summary

- Category guess: C
- Why deferred: Multiple root causes (fragment/code-shape divergence plus pervasive vnode caching differences) tied to known batch caching behavior.
- Suspected modules: `crates/verter_core/src/codegen/vue/template/element.rs`, `crates/verter_core/src/codegen/vue/template_plugin.rs`, `crates/verter_core/src/codegen/vue/template/types.rs`

### Vue output snippet

```js
_createElementVNode("div", _hoisted_1, [
  (_ctx.props.hot)
    ? (_openBlock(), _createElementBlock("div", _hoisted_2, [ ... ]))
    : _createCommentVNode("v-if", true)
])
```

### Verter output snippet

```js
_cache[1] || (_cache[1] = _createElementVNode("div", { class: "flex flex-col ..." }, [
  _ctx.props.hot
    ? (_openBlock(), _createElementBlock(_Fragment, { key: 0 }, [ ... ]))
    : _createCommentVNode("v-if", true)
], -1 /* CACHED */))
```

### Observed diffs

- Verter wraps major static regions in cache slots where Vue does not.
- Verter emits `_Fragment` branches for template conditionals where Vue emits direct element branches.
- Multiple descendants use cached text/element nodes with `-1 /* CACHED */`, diverging from Vue structure.
- Root attr merge behavior differs (`_mergeProps(..., _ctx.$attrs, ...)` missing in Verter output).
