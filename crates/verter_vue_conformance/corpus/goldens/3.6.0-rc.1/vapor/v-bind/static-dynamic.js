import { setProp as _setProp, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'static-dynamic',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const title = ref("Hello")
const disabled = ref(false)

const __returned__ = { title, disabled, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<button type=button>Go", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _renderEffect(() => {
    _setProp(n0, "title", _ctx.title)
    _setProp(n0, "disabled", _ctx.disabled)
  })
  return n0
}
_sfc_main.render = render
export default _sfc_main
