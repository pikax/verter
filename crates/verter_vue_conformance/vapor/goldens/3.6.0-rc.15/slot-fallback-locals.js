import { child as _child, setInsertionState as _setInsertionState, txt as _txt, setProp as _setProp, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createSlot as _createSlot, next as _next, template as _template } from 'vue';
const t0 = _template("<h2> ")
const t1 = _template("<button> </button>")
const t2 = _template("<i>after", 2)
const t3 = _template("<dialog><header><!><span></header><footer>", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n10 = t3()
  const n4 = _child(n10)
  const n3 = _child(n4)
  const n9 = _next(n4)
  _setInsertionState(n4, n3)
  const n0 = _createSlot("head", { titleId: () => (_ctx.titleId) }, () => {
    const n2 = t0()
    const x2 = _txt(n2)
    _renderEffect(() => {
      _setProp(n2, "id", _ctx.titleId)
      _setText(x2, _toDisplayString(_ctx.title))
    })
    return n2
  })
  _setInsertionState(n9)
  const n5 = _createSlot("foot", null, () => {
    const n7 = t1()
    const n8 = t2()
    const x7 = _txt(n7)
    _renderEffect(() => {
      _setProp(n7, "disabled", _ctx.busy)
      _setText(x7, _toDisplayString(_ctx.title))
    })
    return [n7, n8]
  })
  return n10
}
