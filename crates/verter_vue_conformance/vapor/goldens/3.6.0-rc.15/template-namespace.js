import { setInsertionState as _setInsertionState, setAttr as _setAttr, renderEffect as _renderEffect, createIf as _createIf, template as _template } from 'vue';
const t0 = _template("<circle>", 0, 1)
const t1 = _template("<g><rect>", 2, 1)
const t2 = _template("<svg>", 1, 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n5 = t2()
  _setInsertionState(n5)
  const n0 = _createIf(() => (_ctx.open), () => {
    const n2 = t0()
    _renderEffect(() => _setAttr(n2, "r", _ctx.radius, true))
    return n2
  }, () => {
    const n4 = t1()
    return n4
  }, 325 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, FALSE_NO_SCOPE, KEYED_INDEX_0 */)
  return n5
}
