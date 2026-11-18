import { child as _child, next as _next, nthChild as _nthChild, on as _on, txt as _txt, setProp as _setProp, toDisplayString as _toDisplayString, setText as _setText, setAttr as _setAttr, renderEffect as _renderEffect, setInsertionState as _setInsertionState, createAssetComponent as _createAssetComponent, createSlot as _createSlot, createIf as _createIf, template as _template } from 'vue';
const t0 = _template("<p>x", 2)
const t1 = _template("<section><ul><li><i>zero</i><span>one</span><em>two</em><b>three</b><strong>four</strong><u>five</u></ul><p>static</p><div> <b> </b></div><details><summary><!> </summary><div></details><nav><a>one</a><a>two</a><a>three</a><!><b>tail</b><i>end", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n19 = t1()
  let p0 = _child(_child(n19))
  const n0 = _next(_child(p0))
  const n1 = _nthChild(p0, 4)
  const n2 = _next(n1)
  const n5 = _nthChild(n19, 2)
  const n3 = _child(n5, true)
  const n4 = _next(n3)
  let p1 = _next(n5)
  const n9 = _child(p1)
  const n8 = _child(n9)
  const n7 = _next(n8, true)
  const n11 = _next(n9)
  const n18 = _next(p1)
  const n12 = _nthChild(n18, 2)
  const n17 = _next(n12)
  const n16 = _nthChild(n18, 5)
  _on(n2, "click", _ctx.go)
  const x4 = _txt(n4)
  _renderEffect(() => {
    const _label = _ctx.label
    const _note = _ctx.note
    _setProp(n0, "title", _label)
    _setProp(n1, "title", _note)
    _setProp(n5, "title", _ctx.href)
    _setText(n3, _toDisplayString(_label))
    _setText(x4, _toDisplayString(_note))
    _setAttr(n9, "aria-label", _label)
  })
  _setInsertionState(n9, n8)
  const n6 = _createAssetComponent("Icon")
  _renderEffect(() => _setText(n7, _toDisplayString(_ctx.note)))
  _setInsertionState(n11)
  const n10 = _createSlot()
  _on(n11, "click", _ctx.go)
  _renderEffect(() => _setProp(n12, "href", _ctx.href))
  _setInsertionState(n18, n17)
  const n13 = _createIf(() => (_ctx.open), () => {
    const n15 = t0()
    return n15
  }, null, 33 /* TRUE_SINGLE_ROOT, TRUE_NO_SCOPE */)
  _renderEffect(() => _setProp(n16, "title", _ctx.note))
  return n19
}
