import { createTemplateRefSetter as _createTemplateRefSetter, child as _child, setAttr as _setAttr, renderEffect as _renderEffect, setInsertionState as _setInsertionState, createAssetComponent as _createAssetComponent, next as _next, toDisplayString as _toDisplayString, setText as _setText, createSlot as _createSlot, on as _on, setStaticTemplateRef as _setStaticTemplateRef, txt as _txt, createFor as _createFor, template as _template } from 'vue';
const t0 = _template("<details><summary><!> </summary><div>")
const t1 = _template("<li> ")
const t2 = _template("<ul>")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const _setTemplateRef = _createTemplateRefSetter()
  const n6 = t0()
  const n3 = _child(n6)
  const n2 = _child(n3)
  const n1 = _next(n2, true)
  const n5 = _next(n3)
  _renderEffect(() => _setAttr(n3, "aria-label", _ctx.label))
  _setInsertionState(n3, n2)
  const n0 = _createAssetComponent("Icon")
  _renderEffect(() => _setText(n1, _toDisplayString(_ctx.label)))
  _setInsertionState(n5)
  const n4 = _createSlot()
  _on(n5, "click", _ctx.close)
  _setStaticTemplateRef(n6, "menu")
  const n7 = _createAssetComponent("Panel")
  const n11 = t2()
  _setStaticTemplateRef(n7, "panel")
  _setInsertionState(n11)
  const n8 = _createFor(() => (_ctx.items), (_for_item0) => {
    const n10 = t1()
    const x10 = _txt(n10)
    _setTemplateRef(n10, "rows", true)
    _renderEffect(() => _setText(x10, _toDisplayString(_for_item0.value)))
    return n10
  }, (item) => (item), 9 /* FAST_REMOVE, IS_SINGLE_NODE */)
  return [n6, n7, n11]
}
