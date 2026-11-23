import { child as _child, next as _next, nthChild as _nthChild, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, setProp as _setProp, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<section><textarea rows=6></textarea><div class=x> </div><span></span><p>tail", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n2 = t0()
  const n0 = _next(_child(n2))
  const n1 = _nthChild(n2, 3)
  const x0 = _txt(n0)
  _renderEffect(() => {
    _setText(x0, _toDisplayString(_ctx.body))
    _setProp(n1, "title", _ctx.title)
  })
  return n2
}
