import { createSlot as _createSlot, extend as _extend, createDynamicComponent as _createDynamicComponent, template as _template } from 'vue';
const _sfc_main = {}
const t0 = _template("<input type=checkbox>", 2)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = _createDynamicComponent(() => (_ctx.current), { class: "control" }, _extend(() => {
    const n0 = _createSlot()
    return n0
  }, { _: 8 /* NON_STABLE */ }))
  const n2 = t0()
  return [n1, n2]
}
_sfc_main.render = render
export default _sfc_main
