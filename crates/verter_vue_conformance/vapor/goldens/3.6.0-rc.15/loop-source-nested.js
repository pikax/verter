import { setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createFor as _createFor, template as _template } from 'vue';
const t0 = _template("<div> ")
const t1 = _template("<section>")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = _createFor(() => (_ctx.groups), (_for_item0) => {
    const n5 = t1()
    _setInsertionState(n5)
    const n2 = _createFor(() => ((_for_item0.value.children || _ctx.fallback).filter(group => group > 0)), (_for_item1) => {
      const n4 = t0()
      const x4 = _txt(n4)
      _renderEffect(() => _setText(x4, _toDisplayString(_for_item1.value)))
      return n4
    }, undefined, 9 /* FAST_REMOVE, IS_SINGLE_NODE */)
    return n5
  }, undefined, 8 /* IS_SINGLE_NODE */)
  return n0
}
