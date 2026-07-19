import { applyTextModel as _applyTextModel, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'input',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const text = ref("")

const __returned__ = { text, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<input placeholder=\"Type here\">", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _applyTextModel(n0, () => (_ctx.text), _value => (_ctx.text = _value))
  return n0
}
_sfc_main.render = render
export default _sfc_main
