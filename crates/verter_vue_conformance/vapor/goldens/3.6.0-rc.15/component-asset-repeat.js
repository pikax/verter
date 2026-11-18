import { resolveComponent as _resolveComponent, setInsertionState as _setInsertionState, createComponentWithFallback as _createComponentWithFallback, createIf as _createIf, createFor as _createFor, template as _template } from 'vue';
const t0 = _template("<div>", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const _component_Badge = _resolveComponent("Badge")
  const n10 = t0()
  _setInsertionState(n10)
  const n0 = _createIf(() => (_ctx.on), () => {
    const n2 = _createComponentWithFallback(_component_Badge)
    return n2
  }, () => {
    const n4 = _createComponentWithFallback(_component_Badge, { tone: "x" })
    return n4
  }, 261 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, KEYED_INDEX_0 */)
  _setInsertionState(n10, 1)
  const n5 = _createFor(() => (_ctx.rows), (_for_item0) => {
    const n7 = _createComponentWithFallback(_component_Badge, { tone: () => (_for_item0.value) })
    return n7
  }, (row) => (row), 2 /* IS_COMPONENT */)
  _setInsertionState(n10, 2)
  const n8 = _createComponentWithFallback(_component_Badge)
  _setInsertionState(n10, 3)
  const n9 = _createComponentWithFallback(_component_Badge)
  return n10
}
