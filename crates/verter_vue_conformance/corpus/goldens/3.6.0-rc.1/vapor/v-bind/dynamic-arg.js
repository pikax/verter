import { setDynamicProps as _setDynamicProps, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-arg',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const attrName = ref("title")
const value = ref("Tooltip")

const __returned__ = { attrName, value, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<p>Dynamic attribute", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _renderEffect(() => _setDynamicProps(n0, [{ [_ctx.attrName]: _ctx.value }]))
  return n0
}
_sfc_main.render = render
export default _sfc_main
