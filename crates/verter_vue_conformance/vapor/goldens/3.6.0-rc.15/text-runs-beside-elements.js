import { child as _child, nthChild as _nthChild, next as _next, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<div> <p></p> ")
const t1 = _template("<label><input> ")
const t2 = _template("<nav> <i>i</i>tail")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n2 = t0()
  const n0 = _child(n2, true)
  const n1 = _nthChild(n2, 2, true)
  const n4 = t1()
  const n3 = _next(_child(n4), true)
  const n6 = t2()
  const n5 = _child(n6, true)
  _renderEffect(() => {
    _setText(n0, "a " + _toDisplayString(_ctx.first))
    _setText(n1, "b " + _toDisplayString(_ctx.second))
    _setText(n3, _toDisplayString(_ctx.label))
    _setText(n5, _toDisplayString(_ctx.head))
  })
  return [n2, n4, n6]
}
