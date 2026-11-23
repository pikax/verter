import { resolveComponent as _resolveComponent, child as _child, setInsertionState as _setInsertionState, createComponentWithFallback as _createComponentWithFallback, createIf as _createIf, template as _template } from 'vue';
const t0 = _template("<span>x", 2)
const t1 = _template("<div><!> ")
const t2 = _template("<p><!> ")

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const _component_Badge = _resolveComponent("Badge")
  const n7 = t1()
  const n6 = _child(n7)
  _setInsertionState(n7, n6)
  const n0 = _createIf(() => (_ctx.first), () => {
    const n2 = _createComponentWithFallback(_component_Badge)
    return n2
  })
  _setInsertionState(n7, 2)
  const n3 = _createIf(() => (_ctx.second), () => {
    const n5 = t0()
    return n5
  }, null, 33 /* TRUE_SINGLE_ROOT, TRUE_NO_SCOPE */)
  const n11 = t2()
  const n10 = _child(n11)
  _setInsertionState(n11, n10)
  const n8 = _createComponentWithFallback(_component_Badge)
  _setInsertionState(n11, 2)
  const n9 = _createComponentWithFallback(_component_Badge)
  return [n7, n11]
}
