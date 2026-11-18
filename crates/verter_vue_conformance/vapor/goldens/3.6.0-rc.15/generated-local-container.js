import { child as _child, setAttr as _setAttr, renderEffect as _renderEffect, setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, createFor as _createFor, next as _next, on as _on, setProp as _setProp, template as _template } from 'vue';
const t0 = _template("<span> ")
const t1 = _template("<figure><div></div><p><button>go</button><b> </b></p><section><article><em> ", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n8 = t1()
  const n3 = _child(n8)
  let p0 = _next(n3)
  const n4 = _child(p0)
  const n5 = _next(n4)
  const n7 = _child((p0 = _next(p0)))
  const n6 = _child(n7)
  _renderEffect(() => _setAttr(n3, "aria-label", _ctx.label))
  _setInsertionState(n3)
  const n0 = _createFor(() => (_ctx.items), (_for_item0) => {
    const n2 = t0()
    const x2 = _txt(n2)
    _renderEffect(() => _setText(x2, _toDisplayString(_for_item0.value)))
    return n2
  }, (item) => (item), 9 /* FAST_REMOVE, IS_SINGLE_NODE */)
  _on(n4, "click", _ctx.go)
  const x5 = _txt(n5)
  const x6 = _txt(n6)
  _renderEffect(() => {
    const _label = _ctx.label
    _setProp(n4, "disabled", _ctx.busy)
    _setText(x5, _toDisplayString(_label))
    _setProp(n7, "title", _label)
    _setText(x6, _toDisplayString(_ctx.deep))
  })
  return n8
}
