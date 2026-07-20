import { applyVShow as _applyVShow, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'show',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const visible = ref(true)

const __returned__ = { visible, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<p>Peekaboo", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _applyVShow(n0, () => (_ctx.visible))
  return n0
}
_sfc_main.render = render
export default _sfc_main
