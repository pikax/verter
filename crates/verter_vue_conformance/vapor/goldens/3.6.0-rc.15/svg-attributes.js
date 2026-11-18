import { child as _child, setAttr as _setAttr, setClass as _setClass, setStyle as _setStyle, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<svg><circle>", 1, 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = t0()
  const n0 = _child(n1)
  _renderEffect(() => {
    _setAttr(n1, "width", _ctx.width, true)
    _setAttr(n1, "viewBox", _ctx.box, true)
    _setClass(n1, _ctx.tone, true)
    _setAttr(n0, "cx", _ctx.x, true)
    _setAttr(n0, "cy", _ctx.y, true)
    _setAttr(n0, "r", _ctx.radius, true)
    _setStyle(n0, _ctx.fill)
  })
  return n1
}
