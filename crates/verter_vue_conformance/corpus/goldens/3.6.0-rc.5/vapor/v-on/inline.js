import { ref } from "vue"


const _sfc_main = {
  __name: 'inline',
  __vapor: true,
  setup(__props, { expose: __expose }) {
  __expose();

const count = ref(0)

const __returned__ = { count, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { txt as _txt, on as _on, toDisplayString as _toDisplayString, setText as _setText, renderEffect as _renderEffect, template as _template } from 'vue';
const t0 = _template("<button> ", 1)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  const x0 = _txt(n0)
  _on(n0, "click", () => (_ctx.count++))
  _renderEffect(() => _setText(x0, "Count: " + _toDisplayString(_ctx.count)))
  return n0
}
_sfc_main.render = render
export default _sfc_main
