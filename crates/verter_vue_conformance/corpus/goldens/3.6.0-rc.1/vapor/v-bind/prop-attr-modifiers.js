import { setDOMProp as _setDOMProp, setAttr as _setAttr, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'prop-attr-modifiers',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const text = ref("inner")
const dataX = ref("x")

const __returned__ = { text, dataX, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<div>", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _renderEffect(() => {
    _setDOMProp(n0, "text-content", _ctx.text)
    _setAttr(n0, "data-x", _ctx.dataX)
  })
  return n0
}
_sfc_main.render = render
export default _sfc_main
