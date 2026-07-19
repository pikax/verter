import { txt as _txt, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
import { ref } from "vue"


const _sfc_main = {
  __name: 'text',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const plain = ref("plain text")

const __returned__ = { plain, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const t0 = _template("<p> ", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  const x0 = _txt(n0)
  _renderEffect(() => _setText(x0, _toDisplayString(_ctx.plain)))
  return n0
}
_sfc_main.render = render
export default _sfc_main
