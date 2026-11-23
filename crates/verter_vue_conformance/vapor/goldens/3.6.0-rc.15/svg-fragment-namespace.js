import { setAttr as _setAttr, renderEffect as _renderEffect, createAssetComponent as _createAssetComponent, child as _child, setProp as _setProp, template as _template } from 'vue';
const t0 = _template("<path>", 0, 1)
const t1 = _template("<g class=axis><line>", 0, 1)
const t2 = _template("<mi>", 0, 2)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = _createAssetComponent("AppIcon", null, () => {
    const n0 = t0()
    _renderEffect(() => _setAttr(n0, "d", _ctx.d, true))
    return n0
  })
  const n3 = t1()
  const n2 = _child(n3)
  const n4 = t2()
  _renderEffect(() => {
    _setAttr(n2, "x1", _ctx.x, true)
    _setProp(n4, "x", _ctx.y)
  })
  return [n1, n3, n4]
}
