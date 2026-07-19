import { setClass as _setClass, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-class',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const active = ref(true)
const size = ref("lg")

const __returned__ = { active, size, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<div>Classy", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _renderEffect(() => _setClass(n0, ['card', { active: _ctx.active }, 'size-' + _ctx.size]))
  return n0
}
_sfc_main.render = render
export default _sfc_main
