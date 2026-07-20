import { applySelectModel as _applySelectModel, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'select',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const picked = ref("a")

const __returned__ = { picked, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<select><option value=a>A</option><option value=b>B", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _applySelectModel(n0, () => (_ctx.picked), _value => (_ctx.picked = _value))
  return n0
}
_sfc_main.render = render
export default _sfc_main
