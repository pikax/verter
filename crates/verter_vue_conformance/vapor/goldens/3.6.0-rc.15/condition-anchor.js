import { child as _child, setProp as _setProp, renderEffect as _renderEffect, setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, createIf as _createIf, createSlot as _createSlot, next as _next, template as _template } from 'vue';
const t0 = _template("<b> </b>")
const t1 = _template("<header><!><span>x</span>")
const t2 = _template("<section><!><div class=body>", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n11 = t2()
  const n10 = _child(n11)
  const n9 = _next(n10)
  _renderEffect(() => _setProp(n11, "title", _ctx.title))
  _setInsertionState(n11, n10)
  const n0 = _createIf(() => (_ctx.title), () => {
    const n7 = t1()
    const n6 = _child(n7)
    _setInsertionState(n7, n6)
    const n2 = _createIf(() => (_ctx.title), () => {
      const n4 = t0()
      const x4 = _txt(n4)
      _renderEffect(() => _setText(x4, _toDisplayString(_ctx.title)))
      return n4
    })
    _setInsertionState(n7, 2)
    const n5 = _createSlot("actions")
    return n7
  })
  _setInsertionState(n9)
  const n8 = _createSlot()
  return n11
}
