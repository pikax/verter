import { resolveComponent as _resolveComponent, setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createComponentWithFallback as _createComponentWithFallback, setProp as _setProp, template as _template } from 'vue';
const t0 = _template("<p>1</p>", 2)
const t1 = _template("<span> ")
const t2 = _template("t", 2)
const t3 = _template("<b></b>")
const t4 = _template("<i></i>", 2)
const t5 = _template(" ", 2)
const t6 = _template("<em> ")
const t7 = _template("<div>", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const _component_Panel = _resolveComponent("Panel")
  const n10 = t7()
  _setInsertionState(n10)
  const n2 = _createComponentWithFallback(_component_Panel, null, () => {
    const n0 = t0()
    const n1 = t1()
    const x1 = _txt(n1)
    _renderEffect(() => _setText(x1, _toDisplayString(_ctx.count)))
    return [n0, n1]
  })
  _setInsertionState(n10, 1)
  const n9 = _createComponentWithFallback(_component_Panel, null, {
    "head": () => {
      const n3 = t2()
      const n4 = t3()
      _renderEffect(() => _setProp(n4, "title", _ctx.title))
      return [n3, n4]
    },
    "default": () => {
      const n6 = t4()
      const n7 = t5()
      const n8 = t6()
      const x8 = _txt(n8)
      _renderEffect(() => _setText(x8, _toDisplayString(_ctx.note)))
      return [n6, n7, n8]
    }
  })
  return n10
}
