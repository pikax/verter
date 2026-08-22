const _sfc_main = { __vapor: true }
import { createSlot as _createSlot, extend as _extend, createDynamicComponent as _createDynamicComponent, template as _template } from 'vue';
const t0 = _template("<input type=checkbox>", 2)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = _createDynamicComponent(() => (_ctx.current), { class: "control" }, _extend(() => {
    const n0 = _createSlot("default", null, null, 36 /* SLOT_ROOT, INHERIT_FALLBACK */)
    return n0
  }, { _: 8 /* NON_STABLE */ }))
  const n2 = t0()
  return [n1, n2]
}
_sfc_main.render = render
export default _sfc_main
