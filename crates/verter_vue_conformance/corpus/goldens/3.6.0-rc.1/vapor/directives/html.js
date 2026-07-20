import { setHtml as _setHtml, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'html',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const raw = ref("<b>bold</b>")

const __returned__ = { raw, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<div>", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _renderEffect(() => _setHtml(n0, _ctx.raw))
  return n0
}
_sfc_main.render = render
export default _sfc_main
