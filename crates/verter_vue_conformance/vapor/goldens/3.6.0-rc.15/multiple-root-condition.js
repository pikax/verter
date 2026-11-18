import { createIf as _createIf, template as _template } from 'vue';
const t0 = _template("<!-- first -->", 2)
const t1 = _template("<span>one", 2)
const t2 = _template("<!-- second -->", 2)
const t3 = _template("<em>two", 2)
const t4 = _template("<i>three", 2)
const t5 = _template("<p>z", 3)

export function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = _createIf(() => (_ctx.a), () => {
    const n2 = t0()
    const n3 = _createIf(() => (_ctx.b), () => {
      const n5 = t1()
      return n5
    })
    const n6 = t2()
    const n7 = _createIf(() => (_ctx.y), () => {
      const n9 = t3()
      return n9
    }, () => {
      const n11 = t4()
      return n11
    }, 517 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, KEYED_INDEX_1 */)
    return [n2, n3, n6, n7]
  }, () => {
    const n14 = t5()
    return n14
  }, 838 /* TRUE_MULTI_ROOT, FALSE_SINGLE_ROOT, FALSE_NO_SCOPE, KEYED_INDEX_2 */)
  return n0
}
