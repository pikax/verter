import { child as _child, applyCheckboxModel as _applyCheckboxModel, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'checkbox',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const agreed = ref(false)

const __returned__ = { agreed, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<label><input type=checkbox> Agree", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n1 = t0()
  const n0 = _child(n1)
  _applyCheckboxModel(n0, () => (_ctx.agreed), _value => (_ctx.agreed = _value))
  return n1
}
_sfc_main.render = render
export default _sfc_main
