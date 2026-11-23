import { setInsertionState as _setInsertionState, txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, createIf as _createIf, createFor as _createFor, template as _template } from 'vue';
const t0 = _template("<li> ")
const t1 = _template("<li>a")
const t2 = _template("<li>b")
const t3 = _template("<li>c")
const t4 = _template("<ul>", 1)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n16 = t4()
  _setInsertionState(n16)
  const n0 = _createFor(() => (_ctx.items), (_for_item0) => {
    const n2 = _createIf(() => (_for_item0.value.open), () => {
      const n4 = t0()
      const x4 = _txt(n4)
      _renderEffect(() => _setText(x4, _toDisplayString(_for_item0.value.name)))
      return n4
    }, null, 10 /* TRUE_MULTI_ROOT, FALSE_MULTI_ROOT */)
    return n2
  }, undefined, 80 /* IS_FRAGMENT, WRAPPED_ROWS */)
  _setInsertionState(n16, 1)
  const n5 = _createFor(() => (_ctx.rows), (_for_item0) => {
    const n7 = _createIf(() => (_for_item0.value.a), () => {
      const n9 = t1()
      return n9
    }, () => _createIf(() => (_for_item0.value.b), () => {
      const n11 = t2()
      return n11
    }, () => {
      const n14 = t3()
      return n14
    }, 778 /* TRUE_MULTI_ROOT, FALSE_MULTI_ROOT, KEYED_INDEX_2 */), 526 /* TRUE_MULTI_ROOT, FALSE_ELSE_IF, KEYED_INDEX_1 */)
    return n7
  }, undefined, 80 /* IS_FRAGMENT, WRAPPED_ROWS */)
  return n16
}
