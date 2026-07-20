import { setStyle as _setStyle, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-style',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const color = ref("red")
const top = ref(10)

const __returned__ = { color, top, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<div>Styled", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _renderEffect(() => _setStyle(n0, { color: _ctx.color, marginTop: _ctx.top + 'px' }))
  return n0
}
_sfc_main.render = render
export default _sfc_main
