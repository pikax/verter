import { child as _child, next as _next, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<div><p>x</p> ")
const t1 = _template("<p><b>bold</b> ")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = t0()
  const n0 = _next(_child(n1), true)
  const n3 = t1()
  const n2 = _next(_child(n3), true)
  _renderEffect(() => {
    _setText(n0, " " + _toDisplayString(_ctx.count))
    _setText(n2, " tail " + _toDisplayString(_ctx.note))
  })
  return [n1, n3]
}
