import { setProp as _setProp, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'same-name-shorthand',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const id = ref("a1")
const title = ref("Hi")

const __returned__ = { id, title, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<div>", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  _renderEffect(() => {
    _setProp(n0, "id", _ctx.id)
    _setProp(n0, "title", _ctx.title)
  })
  return n0
}
_sfc_main.render = render
export default _sfc_main
